package com.dckj.kkgl.service;

import com.dckj.core.BaseService;
import com.dckj.core.MiniDaoProxy;
import com.dckj.core.Parameter;
import com.dckj.core.utils.PropKit;
import com.dckj.core.utils.SpringContextUtils;
import com.dckj.core.utils.StrKit;
import com.mini.jdbc.Record;
import dckj.core.utils.JcdmUtils;
import edu.emory.mathcs.backport.java.util.Arrays;
import org.apache.commons.lang3.StringUtils;
import org.springframework.stereotype.Service;

import javax.script.ScriptEngine;
import javax.script.ScriptEngineManager;
import javax.script.ScriptException;
import java.util.*;

/**
 * 教学周历提交检查，责任链模式
 */
@Service
public class JxrlAuditPolicyService extends BaseService {

    /**
     * 教学周历提交策略检查
     * @param req
     * @return
     */
    public String policyCheck(Parameter req) {
        boolean istj = "1".equals(req.getPara("istj")); //提交检查
        boolean isss = "1".equals(req.getPara("isss")); //送审检查
        if (!istj && !isss) return null;

        if (isss) istj = false; //检查优先级：送审->提交

        Record kkrw = dao.find("select kcrwdm,xnxqdm,jxbdm,xqllxs,xqsjxs,jxrlauditpolicy,jxrlssauditpolicy from v_k_0001" +
                        " where kcrwdm = ? and xnxqdm = ?",Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));
        if (kkrw == null) return null;
        req.putAll2(kkrw);

        List<String> policyList = new ArrayList<>();
        if (req.hasPara("policylist")) { //选择提交检查策略
            policyList = Arrays.asList(req.getPara("policylist").split(","));
        }
        if (policyList.size() < 1) { //从教学进度中获取
            if (istj && StrKit.notBlank(kkrw.getStr("jxrlauditpolicy"))) {
                policyList = Arrays.asList(kkrw.getStr("jxrlauditpolicy").split(","));
            }
            if (isss && StrKit.notBlank(kkrw.getStr("jxrlssauditpolicy"))) {
                policyList = Arrays.asList(kkrw.getStr("jxrlssauditpolicy").split(","));
            }
        }
        if (policyList.size() < 1) { //字典表获取
            // 使用默认字段代替配置项
            String sqlwhere = "";
            if (istj) sqlwhere += " and istj = '1'";
            if (isss) sqlwhere += " and isss = '1'";
            policyList = dao.findList("select dm from z_0143 where isqy = '1'"+sqlwhere+" order by xh",String.class);
        }
//        if (policyList.size() < 1) { //从配置项获取
//            policyList = Arrays.asList(PropKit.get("ntss.jxrlAuditPolicy").split(","));
//        }

        if (policyList.size() < 1) return null;

        // 设置责任链
        PipelineChain pipelineChain = new PipelineChain();
        if (policyList.contains("notNullSyjxcd")) pipelineChain.addLast(new NotNullSyjxcdHandler());
        if (policyList.contains("notNullJxcd")) pipelineChain.addLast(new NotNullJxcdHandler());
        if (policyList.contains("jxcdrs")) pipelineChain.addLast(new JxcdrsHandler());
        if (policyList.contains("xs")) pipelineChain.addLast(new XsHandler());
        if (policyList.contains("jdzxs")) pipelineChain.addLast(new JdzxsHandler());
        if (policyList.contains("multiTeacher")) pipelineChain.addLast(new MultiTeacherHandler());
        if (policyList.contains("syjxMultiTeacher")) pipelineChain.addLast(new SyjxMultiTeacherHandler());
        if (policyList.contains("jxjd")) pipelineChain.addLast(new JxjdHandler());
        if (policyList.contains("jxjc")) pipelineChain.addLast(new JxjcHandler());
        if (policyList.contains("notNullKb")) pipelineChain.addLast(new NotNullKbHandler());
        if (policyList.contains("notNullTeacher")) pipelineChain.addLast(new NotNullTeacherHandler());
        if (policyList.contains("notNullZdgnqdm")) pipelineChain.addLast(new NotNullZdgnqdmHandler());
        if (policyList.contains("notNullSyxm")) pipelineChain.addLast(new NotNullSyxmHandler());
        if (policyList.contains("notNullSyjxcdsy")) pipelineChain.addLast(new NotNullSyjxcdsyHandler());
        if (policyList.contains("notNullSknr")) pipelineChain.addLast(new NotNullSknrHandler());
        if (policyList.contains("notNullJxms")) pipelineChain.addLast(new NotNullJxmsHandler());
        if (policyList.contains("pyjhnotNullks")) pipelineChain.addLast(new PyjhnotNullksHandler());
        if (policyList.contains("jxcdZt")) pipelineChain.addLast(new JxcdZtHandler());
        if (policyList.contains("notNullJxxs")) pipelineChain.addLast(new NotNullJxxsHandler());
        if (policyList.contains("hxkcNotNight")) pipelineChain.addLast(new HxkcNotNightHandler());
        if (policyList.contains("HxkNotZm")) pipelineChain.addLast(new HxkNotZmHandler());
        if (policyList.contains("jxcdLimit")) pipelineChain.addLast(new JxcdLimitHandler());
        if (policyList.contains("onlineKcLimit")) pipelineChain.addLast(new OnlineKcLimitHandler());
        if (policyList.contains("llkNotKsj")) pipelineChain.addLast(new LlkNotKsjHandler());
        if (policyList.contains("qsjc")) pipelineChain.addLast(new QsjcHandler());
        if (policyList.contains("jcXs")) pipelineChain.addLast(new JcXsHandler());
        if (policyList.contains("syhjQssj")) pipelineChain.addLast(new SyhjQssjHandler());
        if (policyList.contains("llkDayXs")) pipelineChain.addLast(new LlkDayXsHandler());
        if (policyList.contains("xsjxNotJxcd")) pipelineChain.addLast(new XsjxNotJxcdHandler());
        if (policyList.contains("LlkXs")) pipelineChain.addLast(new LlkXsHandler());

        List<String> msgList = new ArrayList<>();
        try {
            pipelineChain.requestProcess(req, msgList);
        } catch (Exception e) {
            e.printStackTrace();
        }
        String msg = "";
        for (int i = 0; i < msgList.size(); i++) {
            if (i > 0) msg += "<br>";
            msg += (i+1)+"、"+msgList.get(i);
        }
        return msg;
    }
}

/**
 * 责任链：处理抽象类
 */
abstract class AbstractHandler {

    MiniDaoProxy dao = SpringContextUtils.getBean(MiniDaoProxy.class);

    abstract void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList);
}

/**
 * 责任链：链路类
 */
class PipelineChain {

    /**
     * 初始化的时候造一个head，作为责任链的开始，但是并没有具体的处理
     * 目的就是启动下一个handler
     */
    public HandlerChainContext head = new HandlerChainContext(new AbstractHandler() {
        @Override
        void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
            handlerChainContext.runNext(req, msgList);
        }
    });

    public void requestProcess(Parameter req, List<String> msgList) {
        this.head.handler(req, msgList);
    }

    public void addLast(AbstractHandler handler) {
        HandlerChainContext context = head;
        while (context.next != null) {
            context = context.next;
        }
        context.next = new HandlerChainContext(handler);
    }
}

/**
 * 责任链：处理类上下文
 */
class HandlerChainContext {

    HandlerChainContext next; // 下一个节点
    AbstractHandler handler;

    public HandlerChainContext(AbstractHandler handler) {
        this.handler = handler;
    }

    void handler(Parameter req, List<String> msgList) {
        this.handler.doHandler(this, req, msgList);
    }

    /**
     * 继续执行下一个
     */
    void runNext(Parameter req, List<String> msgList) {
        if (this.next != null) {
            this.next.handler(req, msgList);
        } else {
//            System.out.println("链条结束");
        }
    }
}

/**
 * 指定了实验功能区，教学场地不允许为空
 */
class NotNullSyjxcdHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        MiniDaoProxy dao = SpringContextUtils.getBean(MiniDaoProxy.class);

        String sql = "select k02.kxh from k_0002 k02"
        +       " inner join g_0009 g09 on k02.zdgnqdm = g09.gnqdm"
        +       " left join g_0008 g08 on k02.jxcddm = g08.jxcddm"
        +       " where k02.kcrwdm = ? and k02.xnxqdm = ?"
        +       " and g09.issys = '1' and k02.jxcddm is null"
        +       " order by k02.kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("已指定实验功能区的教学场地不允许为空，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 教学场地不能为空
 */
class NotNullJxcdHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select k02.kxh from k_0002 k02"
        +       " left join g_0008 g08 on k02.jxcddm = g08.jxcddm"
        +       " where k02.kcrwdm = ? and k02.xnxqdm = ?"
        +       " and g08.jxcdmc is null and k02.bapjxcd = '0'"
        +       " order by k02.kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("排课场地不能为空，请安排排课场地，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 教学场地容纳人数
 */
class JxcdrsHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select k02.kxh from k_0002 k02"
        +       " inner join g_0008 g08 on k02.jxcddm = g08.jxcddm"
        +       " where k02.kcrwdm = ? and k02.xnxqdm = ?"
        +       " and k02.pkrs > g08.rnskrs"
        +       " order by k02.kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("排课人数大于排课场地的容纳人数，请更换排课场地，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 学期学时数检查
 */
class XsHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String xslxSql = "select 1 from k_0002 k02" +
                " inner join k_0009 k09 on k02.kcrwdm = k09.kcrwdm and k02.xnxqdm = k09.xnxqdm" +
                " where k02.kcrwdm = ? and k02.xnxqdm = ? and k02.xslxdm <> k09.xslxdm";
        List<Record> xslxcheck = dao.findList(xslxSql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        String checkSql = "select sum(decode(max(z04.lx),'1',max(k02.xs),0)) as llxs,sum(decode(max(z04.lx),'0',max(k02.xs),0)) as sjxs" +
                " from k_0002 k02" +
                " inner join z_0004 z04 on k02.jxhjdm = z04.dm" +
                " where k02.kcrwdm = ? and k02.xnxqdm = ?" +
                " group by k02.kxh";
        List<Record> licheck = dao.findList(checkSql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        if (licheck.size() > 0 && xslxcheck.size() == 0) {
            float kl = 0F;	//课程学期理论学时
            float ks = 0F;	//课程学期实践学时

            try {
                kl = Float.parseFloat(req.getPara("xqllxs"));
                ks = Float.parseFloat(req.getPara("xqsjxs"));
            } catch (Exception e) {
            }

            float tl = licheck.get(0).getFloat("llxs");	//教师学期安排理论学时
            float ts = licheck.get(0).getFloat("sjxs");	//教师学期安排实践学时
            // 检查学时是否满足要求
            Map<String,Object> m = null;
            try {
                m = _checkXsap(kl, ks, tl, ts);
            } catch (Exception e){
                e.printStackTrace();
                msgList.add("表达式设置有误");
            }

            if (m != null && !(Boolean) m.get("result")) {
                msgList.add((String) m.get("tips"));
            }
        }

        handlerChainContext.runNext(req, msgList);
    }

    /**
     * 检查学时安排是否满足预设规则
     * @param kl 课程学期理论学时
     * @param ks 课程学期实践学时
     * @param tl 教师学期安排理论学时
     * @param ts 教师学期安排实践学时
     * @return
     * @throws ScriptException
     */
    private Map<String, Object> _checkXsap(float kl, float ks, float tl, float ts) throws ScriptException {
        // 默认判断表达式为，x代表课程学期总学时数，y代表教师安排的学时数
        String exp = "Math.abs(x-y) < 2";
        // 默认提示信息
        String tips = "安排学时与本学期要求学时偏差超过允许值2学时!";
        // 检查配置文件是否配置
        String tempExp = PropKit.get("ntss.xsAllowExp");
        String tempTips = PropKit.get("ntss.xsDisallowTips");
        if(StrKit.notBlank(tempExp)) {
            exp = tempExp;
            if(StrKit.notBlank(tempExp)) {
                tips = tempTips;
            } else {
                tips = "安排学时与本学期要求学时偏差不满足提交要求!";
            }
        }
        ScriptEngineManager manager = new ScriptEngineManager();
        ScriptEngine engine = manager.getEngineByName("js");
        engine.put("kl", kl);
        engine.put("ks", ks);
        engine.put("tl", tl);
        engine.put("ts", ts);
        Boolean result = (Boolean) engine.eval(exp);
        Map<String, Object> retMap = new HashMap<>();
        retMap.put("result", result);
        retMap.put("tips", tips);
        return retMap;
    }
}


/**
 * 检查教学进度中的总学时与教学日历安排的学时是否相等
 */
class JdzxsHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        int jdExp = StrKit.notBlank(PropKit.get("ntss.jdAllowExp"))?Integer.parseInt(PropKit.get("ntss.jdAllowExp")):0;

        String param = "";
        if ("SMU_".equals(PropKit.get("ntss.schoolKey"))) {
            param = "zxs";
        } else {
            for (int i = 1; i <= 22; i++) { //zxs22
                param += "+nvl(zxs"+i+",0)";
            }
            param = param.replaceFirst("\\+","");
        }

        String sql = "select abs(a.apxs-b.jdzxs) as pcxs" +
                " from (" +
                    "select nvl(sum(max(xs)),0) as apxs from k_0002 where kcrwdm = ? and xnxqdm = ? group by kxh"//会存在分轮的情况，学时按课序号相同统计
                + ") a" +
                " cross join (" +
                    "select "+param+" as jdzxs from k_0009 where kcrwdm = ? and xnxqdm = ?" +
                ") b";
        List<Record> list = dao.findList(sql, Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"),req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        if (list.size() > 0) {
            int pcxs = list.get(0).getInt("pcxs");
            if (jdExp > 0) {
                if (pcxs > jdExp) {
                    msgList.add("日历安排总学时和进度总学时偏差超过"+jdExp+"!");
                }
            } else {
                if (pcxs > 0) {
                    msgList.add("教学进度安排总学时与教学日历已安排总学时不相符!");
                }
            }
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 多教师安排检查
 */
class MultiTeacherHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select kxh,rs from ("
        +       "select max(k02.kxh) as kxh,count(k06.teadm) as rs"
        +       " from k_0002 k02"
        +       " inner join z_0004 z04 on k02.jxhjdm = z04.dm"
        +       " inner join k_0006 k06 on k02.dgksdm = k06.dgksdm"
        +       " left join z_0074 z74 on k06.cdlbdm = z74.dm"
        +       " where k02.kcrwdm = ? and k02.xnxqdm = ?"
        +       " and z04.lx = '1' and z74.hlct <> '1'"
        +       " group by k02.dgksdm"
        +       ") where rs > 1 order by kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("理论课不允许安排多个授课教师，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 实践教学的课程不允许安排多个授课教师
 */
class SyjxMultiTeacherHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select kxh,rs from ("
        +       "select max(k02.kxh) as kxh,count(k06.teadm) as rs"
        +       " from k_0002 k02"
        +       " inner join z_0004 z04 on k02.jxhjdm = z04.dm"
        +       " inner join k_0006 k06 on k02.dgksdm = k06.dgksdm"
        +       " inner join z_0074 z74 on k06.cdlbdm = z74.dm"
        +       " where k02.kcrwdm = ? and k02.xnxqdm = ?"
        +       " and z04.lx = '0' and z74.hlct <> '1'"
        +       " group by k02.dgksdm"
        +       ") where rs > 1 order by kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("实验教学(教学环节代码02)不允许安排多个授课教师，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 教学进度检查
 */
class JxjdHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String param = "";
        for (int i = 1; i <= 22; i++) { //zxs22
            param += ",zxs"+i;
        }
        param = param.replaceFirst(",","");

        List<Record> lijd = dao.findList("select "+param+" from k_0009 where kcrwdm = ? and xnxqdm = ?",Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        String sql = "select zc,sum(xs) as apxs from (" +
                    "select min(to_number(zc)) zc,max(xs) xs,max(kxh) kxh" +
                    " from k_0002" +
                    " where kcrwdm = ? and xnxqdm = ?" +
                    " group by kxh" +
                ") group by zc";
        List<Record> liapjd = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        if (lijd.size() > 0) {
            for (int i = 0; i < liapjd.size(); i++) {
                String zc = liapjd.get(i).getStr("zc");
                int yapxs = liapjd.get(i).getInt("apxs");
                int jdxs = 0;
                if (StrKit.notBlank(lijd.get(0).getStr("zxs"+zc))) {
                    jdxs = lijd.get(0).getInt("zxs"+zc);
                }
                if (yapxs > jdxs) {
                    msgList.add("第"+zc+"周已安排学时超出进度学时安排,请点击确定重新设定教学进度周学时再提交!");
                }
            }
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 检查教学进程
 */
class JxjcHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        List<Record> zcList = dao.findList("select distinct zc from k_0002 where kcrwdm = ? and xnxqdm = ? and zc is not null",Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        String param = "";
        for (int i = 1; i <= 22; i++) { //jcdm22,apjxjd22
            param += ",g20.jcdm"+i+",g20.apjxjd"+i;
        }
        String sql = "select g11.bjmc"+param
        +       " from g_0020 g20"
        +       " inner join g_0011 g11 on g20.bjdm = g11.bjdm"
        +       " where g20.bjdm in (select bjdm from k_0004 where jxbdm = ?) and g20.xnxqdm = ?";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("jxbdm"),req.getPara("xnxqdm"));

        List<Record> jcList = dao.findList("select dm,dmmc from z_0073",Record.class);
        Map<String,String> jcMap = new HashMap<>();
        for (Record r : jcList) {
            jcMap.put(r.getStr("dm"), r.getStr("dmmc"));
        }

        for (Record r : list) {
            String bjmc = r.getStr("bjmc");
            for (Record zcR : zcList) {
                int zc = zcR.getInt("zc");
                if ("0".equals(r.getStr("apjxjd"+zc))) {
                    msgList.add("根据班级["+bjmc+"]第"+zc+"周教学进程["+jcMap.get(r.getStr("jcdm"+zc))+"]不可排课，请勿删改!");
                }
            }
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 课表不能为空
 */
class NotNullKbHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select kxh from k_0002"
        +       " where kcrwdm = ? and xnxqdm = ?"
        +       " and (zc is null or xq is null or jcdm is null)"
        +       " order by kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("周次，星期，节次不能为空，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 授课教师不能为空
 */
class NotNullTeacherHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select kxh from k_0002"
        +       " where kcrwdm = ? and xnxqdm = ?"
        +       " and teadms is null"
        +       " order by kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("授课教师不能为空，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 功能区必填
 */
class NotNullZdgnqdmHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select kxh from k_0002"
        +       " where kcrwdm = ? and xnxqdm = ?"
        +       " and zdgnqdm is null and bapjxcd <> '1'"
        +       " order by kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("教学功能区不能为空，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 功能区为实验室时，实验项目必填
 */
class NotNullSyxmHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select k02.kxh from k_0002 k02"
        +       " inner join g_0009 g09 on k02.zdgnqdm = g09.gnqdm"
        +       " where k02.kcrwdm = ? and k02.xnxqdm = ?"
        +       " and g09.issys = '1' and k02.syxmdms is null"
        +       " order by k02.kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("在实验室上的课程实验项目不能为空，请选择实验项目，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 功能区为实验室时，场地必填为实验室
 */
class NotNullSyjxcdsyHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select k02.kxh from k_0002 k02"
        +       " inner join g_0009 g09 on k02.zdgnqdm = g09.gnqdm"
        +       " inner join g_0008 g08 on k02.jxcddm = g08.jxcddm"
        +       " inner join g_0009 gz09 on g08.ssgnqdm = gz09.gnqdm"
        +       " where k02.kcrwdm = ? and k02.xnxqdm = ?"
        +       " and g09.issys = '1' and gz09.issys <> '1'"
        +       " order by k02.kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("功能区是实验室的，教室不能为空，教室必须为实验室，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 授课内容不能为空
 */
class NotNullSknrHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select kxh from k_0002"
        +       " where kcrwdm = ? and xnxqdm = ?"
        +       " and sknrjj is null"
        +       " order by kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("授课内容不能为空，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 教学模式不能为空
 */
class NotNullJxmsHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select kxh from k_0002"
        +       " where kcrwdm = ? and xnxqdm = ?"
        +       " and jxmsdm is null"
        +       " order by kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("教学模式不能为空，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 培养计划必须带考试
 */
class PyjhnotNullksHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select 1 from ("
        +           "select k02.kcrwdm,k02.xnxqdm,wm_concat(distinct z04.dmmc) as jxhjmc"
        +           " from k_0002 k02"
        +           " inner join z_0004 z04 on k02.jxhjdm = z04.dm"
        +           " where k02.kcrwdm = ? and k02.xnxqdm = ?"
        +           " group by k02.kcrwdm,k02.xnxqdm"
        +       ") k02"
        +       " inner join k_0001 k01 on k02.kcrwdm = k01.kcrwdm"
        +       " inner join k_0009 k09 on k02.kcrwdm = k09.kcrwdm and k02.xnxqdm = k09.xnxqdm"
        +       " where k01.jhlxdm = '01' and k09.khfsdm = '01' and k02.jxhjmc not like '%考试%'";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        if (list.size() > 0) {
            msgList.add("培养计划课程考核方式是考试的，日历需安排考试环节!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 使用的教学场地为不可用状态
 */
class JxcdZtHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select k02.kxh from k_0002 k02"
        +       " left join g_0008 g08 on k02.jxcddm = g08.jxcddm"
        +       " where k02.kcrwdm = ? and k02.xnxqdm = ?"
        +       " and g08.zt = '0' and k02.bapjxcd = '0'"
        +       " order by k02.kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("场地为不可用状态，请重新安排排课场地，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 教学形式不能为空
 */
class NotNullJxxsHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select kxh from k_0002"
        +       " where kcrwdm = ? and xnxqdm = ?"
        +       " and jxxsdm is null"
        +       " order by kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("教学形式不能为空，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 核心课理论环节线下教学不能晚上
 */
class HxkcNotNightHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select k02.kxh"
        +       " from k_0002 k02"
        +       " inner join k_0001 k01 on k02.kcrwdm = k01.kcrwdm"
        +       " inner join j_0002 j02 on k01.rwdm = j02.rwdm"
        +       " inner join z_0004 z04 on k02.jxhjdm = z04.dm"
        +       " inner join z_0137 z137 on k02.jxxsdm = z137.dm"
        +       " inner join p_0002 p02 on not(k02.ps > p02.jsjc or k02.pe < p02.qsjc)"
        +       " where k02.kcrwdm = ? and k02.xnxqdm = ?"
        +       " and j02.ishxkc = '1' and z04.lx = '1' and z137.dmmc = '线下教学' and k02.jcdm is not null and p02.fzmc = '晚上'"
        +       " order by k02.kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("核心课理论环节线下教学不能晚上的节次，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 核心课不在周末上课
 */
class HxkNotZmHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select k02.kxh"
                +       " from k_0002 k02"
                +       " inner join k_0001 k01 on k02.kcrwdm = k01.kcrwdm"
                +       " inner join j_0002 j02 on k01.rwdm = j02.rwdm"
                +       " where k02.kcrwdm = ? and k02.xnxqdm = ?"
                +       " and j02.ishxkc = '1' and xq in ('6','7')"
                +       " order by k02.kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("核心课不在周末上课，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 一周内安排相同场地不超过3个半天
 */
class JxcdLimitHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select zc from ( " +
                " select zc,count(*) as num " +
                " from k_0002 k02 " +
                "          inner join k_0001 k01 on k02.kcrwdm = k01.kcrwdm " +
                " where k02.kcrwdm = '"+req.getPara("kcrwdm")+"' and k02.xnxqdm = '"+req.getPara("xnxqdm")+"' " +
                "           and (jcdm2 like '%01%' or jcdm2 like '%02%' or jcdm2 like '%03%' or jcdm2 like '%04%' or jcdm2 like '%05%') and k02.jxcddm is not null" +
                " group by zc " +
                " union all " +
                " select zc,count(*) as num " +
                " from k_0002 k02 " +
                "          inner join k_0001 k01 on k02.kcrwdm = k01.kcrwdm " +
                " where k02.kcrwdm = '"+req.getPara("kcrwdm")+"' and k02.xnxqdm = '"+req.getPara("xnxqdm")+"' " +
                "   and (jcdm2 like '%06%' or jcdm2 like '%07%' or jcdm2 like '%08%' or jcdm2 like '%09%') and k02.jxcddm is not null" +
                " group by zc " +
                " union all " +
                " select zc,count(*) as num " +
                " from k_0002 k02 " +
                "          inner join k_0001 k01 on k02.kcrwdm = k01.kcrwdm " +
                " where k02.kcrwdm = '"+req.getPara("kcrwdm")+"' and k02.xnxqdm = '"+req.getPara("xnxqdm")+"' " +
                "   and (jcdm2 like '%10%' or jcdm2 like '%11%' or jcdm2 like '%12%' or jcdm2 like '%13%') and k02.jxcddm is not null" +
                " group by zc " +
                " ) group by zc having sum(num) >= 2 order by zc";
        List<Record> list = dao.findList(sql,Record.class);

        Set<String> zcSet = new TreeSet<>();
        for (Record r : list) {
            zcSet.add(r.getStr("zc"));
        }
        if (zcSet.size() > 0) {
            msgList.add("周次："+StringUtils.join(zcSet,",")+"排课占用超过3个半天，不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}
/**
 * 一周内安排相同场地不超过3个半天
 */
class OnlineKcLimitHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = " select jxxsdm from k_0002 k02 where k02.kcrwdm = '"+req.getPara("kcrwdm")+"' and k02.xnxqdm = '"+req.getPara("xnxqdm")+"' and kxh = '1'";
        Record r = dao.find(sql,Record.class);
        if (r!=null&&r.getStr("jxxsdm").equals("02")){
            msgList.add("第一次课不能是线上!");
        }
        sql = "select sum(xs) as zxs, sum(case when jxxsdm = '02' then xs else 0 end) as xsxs " +
                " from k_0002 k02 " +
                " where k02.kcrwdm = '"+req.getPara("kcrwdm")+"' and k02.xnxqdm = '"+req.getPara("xnxqdm")+"'";
        Record xsrec = dao.find(sql,Record.class);
        if (xsrec!=null&&(xsrec.getDouble("xsxs")/xsrec.getDouble("zxs"))>0.5){
            msgList.add("线上学时不能超过50%。");
        }
        handlerChainContext.runNext(req, msgList);
    }
}


/**
 * 理论课不能跨时间段排课
 */
class LlkNotKsjHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select k02.dgksdm,k02.kxh,p02.fzdm,p02.fzmc"
        +       " from k_0002 k02"
        +       " inner join z_0004 z04 on k02.jxhjdm = z04.dm"
        +       " inner join p_0002 p02 on not(k02.ps > p02.jsjc or k02.pe < p02.qsjc)"
        +       " where k02.kcrwdm = ? and k02.xnxqdm = ?"
        +       " and z04.lx = '1' and k02.jcdm is not null and p02.fzdm is not null"
        +       " order by k02.kxh,k02.dgksdm";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Map<String,String> map = new HashMap<>();
        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            String dgksdm = r.getStr("dgksdm");
            String fzdm = r.getStr("fzdm");
            if (!map.containsKey(dgksdm)) {
                map.put(dgksdm, fzdm);
            } else if (!StrKit.equals(map.get(dgksdm),fzdm)) { //同一课次出现两个时间分组
                kxhSet.add(r.getStr("kxh"));
            }
        }
        if (kxhSet.size() > 0) {
            msgList.add("理论教学环节不允许跨时间段排课，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 起始节次检查
 */
class QsjcHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select kxh from k_0002"
        +       " where kcrwdm = ? and xnxqdm = ?"
        +       " and xslxdm = '01' and xs >= 2 and ps in ('02','07','11')"
        +       " order by kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("起始节次不允许为02,07,11节，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 节次数与学时数要求一致（理论环节学时类型课表）
 */
class JcXsHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select k02.dgksdm,k02.kxh from k_0002 k02"
        +       " inner join z_0004 z04 on k02.jxhjdm = z04.dm"
        +       " where k02.kcrwdm = ? and k02.xnxqdm = ?"
        +       " and z04.lx = '1' and k02.xslxdm = '01' and nvl2(k02.jcdm,length(k02.jcdm)/2,0) <> k02.xs"
        +       " order by k02.kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> dgksSet = new HashSet<>();
        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            dgksSet.add(r.getStr("dgksdm"));
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            dao.execute("update k_0002 set xq = '',jcdm = '',jcdm2 = '',ps = '',pe = '',qsrq = '',jsrq = '',qssj = '',jssj = ''"
            +       " where dgksdm in ('"+StringUtils.join(dgksSet,"','")+"')");

            msgList.add("课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+"的节次数与学时不一致，已自动清空星期和节次!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 实验环节起始时间检查
 */
class SyhjQssjHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select kxh,ps from k_0002"
        +       " where jxhjdm in ('02','03','05','10','11')" //02-实验教学,03-实习见习,05-仅实验考试,10-实践环节,11-床旁教学
        +       " and kcrwdm = ? and xnxqdm = ?"
        +       " and ((ps = '01' and qssj < '08:30') or (ps = '06' and qssj < '13:30') or (ps = '10' and qssj < '18:00'))"
        +       " order by ps,kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Map<String,Set<String>> psMap = new LinkedHashMap<>();
        for (Record r : list) {
            String ps = r.getStr("ps");
            String kxh = r.getStr("kxh");

            if (!psMap.containsKey(ps)) {
                Set<String> kxhSet = new TreeSet<>();
                kxhSet.add(kxh);
                psMap.put(ps, kxhSet);
            } else {
                Set<String> kxhSet = psMap.get(ps);
                kxhSet.add(kxh);
            }
        }
        if (!psMap.isEmpty()) {
            String msg = "";
            for (String ps : psMap.keySet()) {
                if ("01".equals(ps)) {
                    msg += "<br>第1节次开始时间不能早于8:30";
                } else if ("06".equals(ps)) {
                    msg += "<br>第6节次开始时间不能早于13:30";
                } else if ("10".equals(ps)) {
                    msg += "<br>第10节次开始时间不能早于18:00";
                }
                msg += "，课序号"+JcdmUtils.getJcSummary(StringUtils.join(psMap.get(ps),","))+" 不符合要求!";
            }
            msgList.add("实验环节起始时间："+msg);
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 线下教学理论学时一天不超过3学时(学时类型课表)
 */
class LlkDayXsHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select max(k02.qsrq) as qsrq,max(k02.xs) as xs"
        +       " from k_0002 k02"
        +       " inner join k_0003 k03 on k02.jxbdm = k03.jxbdm"
        +       " inner join z_0004 z04 on k02.jxhjdm = z04.dm"
        +       " inner join z_0137 z137 on k02.jxxsdm = z137.dm"
        +       " where k02.kcrwdm = ? and k02.xnxqdm = ?"
        +       " and z04.lx = '1' and z137.dmmc = '线下教学' and k02.xslxdm = '01' and k02.qsrq is not null"
        +       " group by nvl(k03.groupdm,k02.dgksdm)"
        +       " order by qsrq";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Map<String,Double> rqMap = new LinkedHashMap<>();
        for (Record r : list) {
            String qsrq = r.getStr("qsrq");
            double xs = r.getDouble("xs");

            if (!rqMap.containsKey(qsrq)) {
                rqMap.put(qsrq, xs);
            } else {
                rqMap.put(qsrq, rqMap.get(qsrq)+xs);
            }
        }

        String msg = "";
        for (String qsrq : rqMap.keySet()) {
            double xs = rqMap.get(qsrq);

            if (xs > 3) {
                msg += ","+qsrq+"("+xs+")";
            }
        }

        if (StrKit.notBlank(msg)) {
            msgList.add("线下教学理论学时一天不超过3学时，"+msg.replaceFirst(",","")+" 不符合要求");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 线上教学不使用教学场地
 */
class XsjxNotJxcdHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select k02.kxh from k_0002 k02"
        +       " inner join z_0137 z137 on k02.jxxsdm = z137.dm"
        +       " where k02.kcrwdm = ? and k02.xnxqdm = ?"
        +       " and z137.dmmc = '线上教学' and k02.jxcddm is not null"
        +       " order by k02.kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("线上教学不使用教学场地，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}

/**
 * 线下教学形式的理论环节不允许超过3学时
 */
class LlkXsHandler extends AbstractHandler {
    @Override
    void doHandler(HandlerChainContext handlerChainContext, Parameter req, List<String> msgList) {
        String sql = "select k02.kxh"
        +       " from k_0002 k02"
        +       " inner join z_0004 z04 on k02.jxhjdm = z04.dm"
        +       " inner join z_0137 z137 on k02.jxxsdm = z137.dm"
        +       " where k02.kcrwdm = ? and k02.xnxqdm = ?"
        +       " and z04.lx = '1' and z137.dmmc = '线下教学' and k02.xslxdm = '01' and k02.xs > 3"
        +       " order by k02.kxh";
        List<Record> list = dao.findList(sql,Record.class,req.getPara("kcrwdm"),req.getPara("xnxqdm"));

        Set<String> kxhSet = new TreeSet<>();
        for (Record r : list) {
            kxhSet.add(r.getStr("kxh"));
        }
        if (kxhSet.size() > 0) {
            msgList.add("线下教学形式的理论环节不允许超过3学时，课序号"+JcdmUtils.getJcSummary(StringUtils.join(kxhSet,","))+" 不符合要求!");
        }

        handlerChainContext.runNext(req, msgList);
    }
}